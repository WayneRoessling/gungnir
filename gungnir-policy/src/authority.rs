//! Weapons control status and engagement authority.
//!
//! Design: docs/design/DN-09-authority-and-control-status.md, **signed by the owner
//! on 2026-09-05**. Capabilities CAP-3.6 and CAP-6.2; measure MOP-38.
//!
//! **Human-owned** (docs/agentic-workflow.md): this file decides who may engage
//! what. Agents may draft; a change merges only with the owner's sign-off, and a
//! change to the design behind it is a change request under phase H.
//!
//! Two properties hold here and are tested rather than assumed:
//!
//! 1. **No verdict is ever "permitted to act".** The most either engine returns is
//!    [`PolicyVerdict::RequiresHumanApproval`], which is contract C-01.
//! 2. **A denial explains itself.** Each new [`DenialReason`] carries the layer, the
//!    status, or the role that would be required, because "denied" without a reason
//!    sends the operator to a radio to ask why.
//!
//! No new dependency edge: roles are matched by name against
//! `AuthoritySettings`, which `gungnir-config` validates against the adopted set at
//! load, so a misspelling is reported to a person rather than silently granting
//! nothing.

use crate::{DenialReason, PolicyEngine, PolicyVerdict};
use gungnir_model::{
    AuthoritySettings, Classification, ControlStatusSettings, EffectorLayer, PlanView,
    ResourceView, WeaponsControlStatus,
};

/// Denies a plan whose effector layer is at a status that forbids it.
pub struct ControlStatusPolicy<'a> {
    pub settings: &'a ControlStatusSettings,
    /// Classification of each track the plan names, supplied by the caller; the
    /// plan itself carries identifiers rather than classifications.
    pub track_classification: &'a (dyn Fn(gungnir_model::TrackId) -> Classification + Send + Sync),
}

impl ControlStatusPolicy<'_> {
    /// Whether this layer's status permits engaging a track of this classification.
    ///
    /// An unconfigured layer is at `Hold` (docs/design/DN-08-policy-configuration.md).
    fn permits(status: WeaponsControlStatus, classification: Classification) -> bool {
        match status {
            WeaponsControlStatus::Free => classification != Classification::Friendly,
            WeaponsControlStatus::Tight => classification == Classification::Hostile,
            WeaponsControlStatus::Hold => false,
        }
    }
}

impl PolicyEngine for ControlStatusPolicy<'_> {
    fn evaluate(&self, plan: &PlanView, resources: &[ResourceView]) -> PolicyVerdict {
        if plan.is_empty() {
            return PolicyVerdict::Denied {
                reason_code: DenialReason::EmptyPlan,
            };
        }
        // Judged per solution; the plan is denied if any solution is.
        for (resource_id, track_id) in plan.assignments() {
            let Some(resource) = resources.iter().find(|r| r.id == resource_id) else {
                return PolicyVerdict::Denied {
                    reason_code: DenialReason::UnknownResource,
                };
            };
            let status = self.settings.for_layer(resource.layer);
            if !Self::permits(status, (self.track_classification)(track_id)) {
                return PolicyVerdict::Denied {
                    reason_code: DenialReason::ControlStatus {
                        layer: resource.layer,
                        status,
                    },
                };
            }
        }
        // Clean against control status still requires a person. C-01.
        PolicyVerdict::RequiresHumanApproval
    }
}

/// Denies a plan the asking role may not accept, given the layer and the
/// classification of the track.
///
/// The chain is evaluated for a specific asking role, so the same plan can be
/// actionable for a supervisor and not for an operator. That is the point: the
/// queue shows an operator what they may decide and marks what must go up.
pub struct AuthorityPolicy<'a> {
    pub settings: &'a AuthoritySettings,
    /// Canonical role name of the operator asking. Validated against the adopted
    /// set when the baseline loads.
    pub asking_role: &'a str,
    /// The authorization action this decision falls under, from
    /// `gungnir_security::actions`.
    pub action: &'a str,
    pub track_classification: &'a (dyn Fn(gungnir_model::TrackId) -> Classification + Send + Sync),
}

impl AuthorityPolicy<'_> {
    /// The class name the authority matrix keys on. Kept here rather than on the
    /// model so the vocabulary can change without a schema change.
    fn class_name(classification: Classification) -> &'static str {
        match classification {
            Classification::Unknown => "unknown",
            Classification::Neutral => "neutral",
            Classification::Friendly => "friendly",
            Classification::Hostile => "hostile",
        }
    }
}

impl PolicyEngine for AuthorityPolicy<'_> {
    fn evaluate(&self, plan: &PlanView, resources: &[ResourceView]) -> PolicyVerdict {
        if plan.is_empty() {
            return PolicyVerdict::Denied {
                reason_code: DenialReason::EmptyPlan,
            };
        }
        for (resource_id, track_id) in plan.assignments() {
            let Some(resource) = resources.iter().find(|r| r.id == resource_id) else {
                return PolicyVerdict::Denied {
                    reason_code: DenialReason::UnknownResource,
                };
            };
            let class = Self::class_name((self.track_classification)(track_id));
            // No matching rule means denied: an action nobody is granted is an
            // action nobody may take.
            if !self.settings.permits(
                self.action,
                self.asking_role,
                Some(resource.layer),
                Some(class),
            ) {
                return PolicyVerdict::Denied {
                    reason_code: DenialReason::Authority {
                        layer: resource.layer,
                    },
                };
            }
        }
        PolicyVerdict::RequiresHumanApproval
    }
}

/// True when a plan is pre-delegated for this role at every layer it names.
///
/// Decision D-15 pre-delegated one specific case. A pre-delegated plan still needs
/// a recorded decision; what it skips is escalation, not the person.
pub fn is_pre_delegated(
    settings: &AuthoritySettings,
    action: &str,
    role: &str,
    plan: &PlanView,
    resources: &[ResourceView],
    class_of: &(dyn Fn(gungnir_model::TrackId) -> Classification + Send + Sync),
) -> bool {
    let mut any = false;
    for (resource_id, track_id) in plan.assignments() {
        let Some(resource) = resources.iter().find(|r| r.id == resource_id) else {
            return false;
        };
        let class = AuthorityPolicy::class_name(class_of(track_id));
        match settings.rule_for(action, role, Some(resource.layer), Some(class)) {
            Some(matched) if matched.pre_delegated => any = true,
            _ => return false,
        }
    }
    any
}

/// Which roles could accept this plan, for the queue's escalation marks.
pub fn roles_permitting(
    settings: &AuthoritySettings,
    action: &str,
    layer: EffectorLayer,
    class: &str,
) -> Vec<String> {
    let mut roles: Vec<String> = settings
        .rules
        .iter()
        .filter(|r| {
            settings
                .rule_for(action, &r.role, Some(layer), Some(class))
                .is_some()
        })
        .map(|r| r.role.clone())
        .collect();
    roles.sort_unstable();
    roles.dedup();
    roles
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{
        AuthorityRule, Geodetic, InterceptSolutionView, Magazine, PlanId, RelativeCost, ResourceId,
        TrackId,
    };

    fn resource(id: u32, layer: EffectorLayer) -> ResourceView {
        ResourceView {
            id: ResourceId(id),
            position: Geodetic {
                lat_rad: 0.0,
                lon_rad: 0.0,
                alt_m: 0.0,
            },
            capacity: 1,
            ready: true,
            layer,
            cost: RelativeCost::default(),
            magazine: Some(Magazine {
                rounds_available: 4,
                reserve: 1,
            }),
            intercept_speed_mps: None,
        }
    }

    fn plan(resource: u32, track: u64) -> PlanView {
        PlanView {
            id: PlanId(1),
            mission_time: gungnir_model::MissionTime(0.0),
            kind: gungnir_model::PlanKind::Intercept {
                solutions: vec![InterceptSolutionView {
                    resource: ResourceId(resource),
                    track: TrackId(track),
                    intercept_point: None,
                    time_to_intercept_s: None,
                }],
            },
            policy_value: 1.0,
            releasability: gungnir_model::Releasability::default(),
        }
    }

    fn rule(action: &str, role: &str, layer: EffectorLayer, class: &str) -> AuthorityRule {
        AuthorityRule {
            action: action.into(),
            role: role.into(),
            layer: Some(layer),
            class: Some(class.into()),
            pre_delegated: false,
        }
    }

    #[test]
    fn an_unconfigured_layer_is_at_hold_and_denies() {
        let settings = ControlStatusSettings::default();
        let class_of = |_: TrackId| Classification::Hostile;
        let policy = ControlStatusPolicy {
            settings: &settings,
            track_classification: &class_of,
        };
        let verdict = policy.evaluate(&plan(1, 10), &[resource(1, EffectorLayer::Point)]);
        assert!(matches!(
            verdict,
            PolicyVerdict::Denied {
                reason_code: DenialReason::ControlStatus {
                    status: WeaponsControlStatus::Hold,
                    ..
                }
            }
        ));
    }

    #[test]
    fn tight_permits_only_a_hostile_track() {
        let mut settings = ControlStatusSettings::default();
        settings
            .by_layer
            .insert(EffectorLayer::Point, WeaponsControlStatus::Tight);
        let resources = [resource(1, EffectorLayer::Point)];

        for (class, permitted) in [
            (Classification::Hostile, true),
            (Classification::Unknown, false),
            (Classification::Neutral, false),
            (Classification::Friendly, false),
        ] {
            let class_of = move |_: TrackId| class;
            let policy = ControlStatusPolicy {
                settings: &settings,
                track_classification: &class_of,
            };
            let verdict = policy.evaluate(&plan(1, 10), &resources);
            assert_eq!(
                matches!(verdict, PolicyVerdict::RequiresHumanApproval),
                permitted,
                "{class:?} under Tight"
            );
        }
    }

    #[test]
    fn free_permits_anything_that_is_not_friendly() {
        let mut settings = ControlStatusSettings::default();
        settings
            .by_layer
            .insert(EffectorLayer::Area, WeaponsControlStatus::Free);
        let resources = [resource(1, EffectorLayer::Area)];

        let unknown = |_: TrackId| Classification::Unknown;
        let policy = ControlStatusPolicy {
            settings: &settings,
            track_classification: &unknown,
        };
        assert!(matches!(
            policy.evaluate(&plan(1, 10), &resources),
            PolicyVerdict::RequiresHumanApproval
        ));

        let friendly = |_: TrackId| Classification::Friendly;
        let policy = ControlStatusPolicy {
            settings: &settings,
            track_classification: &friendly,
        };
        assert!(matches!(
            policy.evaluate(&plan(1, 10), &resources),
            PolicyVerdict::Denied { .. }
        ));
    }

    #[test]
    fn a_clean_plan_still_requires_a_human() {
        // Contract C-01: nothing here ever returns Approved.
        let mut settings = ControlStatusSettings::default();
        settings
            .by_layer
            .insert(EffectorLayer::Point, WeaponsControlStatus::Free);
        let class_of = |_: TrackId| Classification::Hostile;
        let policy = ControlStatusPolicy {
            settings: &settings,
            track_classification: &class_of,
        };
        assert_eq!(
            policy.evaluate(&plan(1, 10), &[resource(1, EffectorLayer::Point)]),
            PolicyVerdict::RequiresHumanApproval
        );
    }

    #[test]
    fn a_role_with_no_matching_rule_is_denied() {
        let settings = AuthoritySettings::default();
        let class_of = |_: TrackId| Classification::Hostile;
        let policy = AuthorityPolicy {
            settings: &settings,
            asking_role: "operator",
            action: "plan.decide",
            track_classification: &class_of,
        };
        assert!(matches!(
            policy.evaluate(&plan(1, 10), &[resource(1, EffectorLayer::Area)]),
            PolicyVerdict::Denied {
                reason_code: DenialReason::Authority { .. }
            }
        ));
    }

    #[test]
    fn the_same_plan_can_be_actionable_for_one_role_and_not_another() {
        // The authority matrix reserves the area layer to the supervisor.
        let settings = AuthoritySettings {
            rules: vec![
                rule("plan.decide", "supervisor", EffectorLayer::Area, "hostile"),
                rule("plan.decide", "operator", EffectorLayer::Point, "hostile"),
            ],
        };
        let class_of = |_: TrackId| Classification::Hostile;
        let resources = [resource(1, EffectorLayer::Area)];

        let operator = AuthorityPolicy {
            settings: &settings,
            asking_role: "operator",
            action: "plan.decide",
            track_classification: &class_of,
        };
        assert!(matches!(
            operator.evaluate(&plan(1, 10), &resources),
            PolicyVerdict::Denied { .. }
        ));

        let supervisor = AuthorityPolicy {
            settings: &settings,
            asking_role: "supervisor",
            action: "plan.decide",
            track_classification: &class_of,
        };
        assert_eq!(
            supervisor.evaluate(&plan(1, 10), &resources),
            PolicyVerdict::RequiresHumanApproval
        );
    }

    #[test]
    fn every_cell_of_a_small_authority_matrix_is_honoured() {
        // MOP-38 in miniature: the matrix is the fixture, and every cell is
        // exercised rather than sampled.
        let settings = AuthoritySettings {
            rules: vec![
                rule("plan.decide", "operator", EffectorLayer::Point, "hostile"),
                rule("plan.decide", "supervisor", EffectorLayer::Point, "hostile"),
                rule("plan.decide", "supervisor", EffectorLayer::Area, "hostile"),
            ],
        };
        let expected = [
            ("operator", EffectorLayer::Point, true),
            ("operator", EffectorLayer::Area, false),
            ("supervisor", EffectorLayer::Point, true),
            ("supervisor", EffectorLayer::Area, true),
            ("analyst", EffectorLayer::Point, false),
            ("analyst", EffectorLayer::Area, false),
        ];
        let class_of = |_: TrackId| Classification::Hostile;
        for (role, layer, permitted) in expected {
            let policy = AuthorityPolicy {
                settings: &settings,
                asking_role: role,
                action: "plan.decide",
                track_classification: &class_of,
            };
            let verdict = policy.evaluate(&plan(1, 10), &[resource(1, layer)]);
            assert_eq!(
                matches!(verdict, PolicyVerdict::RequiresHumanApproval),
                permitted,
                "{role} at {layer:?}"
            );
        }
    }

    /// MOP-38 over both qualifiers (GAP-058): every role, layer and class cell of a
    /// matrix that grants by class as well as by layer is exercised, none sampled.
    /// **Signed by the owner 2026-09-06** (this crate is human-owned).
    #[test]
    fn every_cell_of_a_class_and_layer_matrix_is_honoured() {
        let settings = AuthoritySettings {
            rules: vec![
                rule("plan.decide", "operator", EffectorLayer::Point, "hostile"),
                rule("plan.decide", "supervisor", EffectorLayer::Point, "hostile"),
                rule("plan.decide", "supervisor", EffectorLayer::Point, "unknown"),
                rule("plan.decide", "commander", EffectorLayer::Area, "hostile"),
            ],
        };
        let classes = [
            ("hostile", Classification::Hostile),
            ("unknown", Classification::Unknown),
            ("neutral", Classification::Neutral),
        ];
        let roles = ["operator", "supervisor", "commander", "analyst"];
        let layers = [EffectorLayer::Point, EffectorLayer::Area];
        let mut cells = 0;
        for role in roles {
            for layer in layers {
                for (class_name, class) in classes {
                    let expected = matches!(
                        (role, layer, class_name),
                        ("operator", EffectorLayer::Point, "hostile")
                            | ("supervisor", EffectorLayer::Point, "hostile" | "unknown")
                            | ("commander", EffectorLayer::Area, "hostile")
                    );
                    let class_of = move |_: TrackId| class;
                    let policy = AuthorityPolicy {
                        settings: &settings,
                        asking_role: role,
                        action: "plan.decide",
                        track_classification: &class_of,
                    };
                    let verdict = policy.evaluate(&plan(1, 10), &[resource(1, layer)]);
                    assert_eq!(
                        matches!(verdict, PolicyVerdict::RequiresHumanApproval),
                        expected,
                        "{role} at {layer:?} against {class_name}"
                    );
                    cells += 1;
                }
            }
        }
        assert_eq!(cells, 24, "the whole matrix, not a sample");
    }

    #[test]
    fn a_denial_names_the_layer_so_the_panel_can_explain_it() {
        let settings = AuthoritySettings::default();
        let class_of = |_: TrackId| Classification::Hostile;
        let policy = AuthorityPolicy {
            settings: &settings,
            asking_role: "operator",
            action: "plan.decide",
            track_classification: &class_of,
        };
        match policy.evaluate(&plan(1, 10), &[resource(1, EffectorLayer::SelfDefence)]) {
            PolicyVerdict::Denied {
                reason_code: DenialReason::Authority { layer },
            } => assert_eq!(layer, EffectorLayer::SelfDefence),
            other => panic!("expected an authority denial naming the layer, got {other:?}"),
        }
    }

    #[test]
    fn pre_delegation_is_recognized_only_where_the_rule_says_so() {
        let mut delegated = rule("plan.decide", "operator", EffectorLayer::Point, "hostile");
        delegated.pre_delegated = true;
        let settings = AuthoritySettings {
            rules: vec![delegated],
        };
        let class_of = |_: TrackId| Classification::Hostile;
        let resources = [resource(1, EffectorLayer::Point)];
        assert!(is_pre_delegated(
            &settings,
            "plan.decide",
            "operator",
            &plan(1, 10),
            &resources,
            &class_of
        ));
        // A different layer is not covered by that rule.
        assert!(!is_pre_delegated(
            &settings,
            "plan.decide",
            "operator",
            &plan(1, 10),
            &[resource(1, EffectorLayer::Area)],
            &class_of
        ));
    }

    #[test]
    fn the_queue_can_ask_which_roles_could_accept() {
        let settings = AuthoritySettings {
            rules: vec![
                rule("plan.decide", "supervisor", EffectorLayer::Area, "hostile"),
                rule("plan.decide", "commander", EffectorLayer::Area, "hostile"),
                rule("plan.decide", "operator", EffectorLayer::Point, "hostile"),
            ],
        };
        let roles = roles_permitting(&settings, "plan.decide", EffectorLayer::Area, "hostile");
        assert_eq!(
            roles,
            vec!["commander".to_string(), "supervisor".to_string()]
        );
    }
}
