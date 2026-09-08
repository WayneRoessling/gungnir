// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Role/attribute-based authorization -- what gungnir-command's approval workflow
//! and gungnir-config's baseline-apply both check before acting.

use crate::{actions, OperatorId, Role, SecurityError};
use std::collections::HashMap;

pub trait Authorizer: Send + Sync {
    fn role(&self, operator: OperatorId) -> Option<Role>;
    fn can(&self, operator: OperatorId, action: &str) -> bool;

    /// `Ok` or a `Forbidden` error naming the action, for call sites that want to
    /// propagate.
    fn require(&self, operator: OperatorId, action: &str) -> Result<(), SecurityError> {
        if self.can(operator, action) {
            Ok(())
        } else {
            Err(SecurityError::Forbidden(action.to_string()))
        }
    }
}

/// The role-to-action matrix. Administrators may do everything.
pub fn role_permits(role: Role, action: &str) -> bool {
    use actions::{
        APPLY_CONFIG, DECIDE_PLAN, EXPORT_REPORT, KEY_ESCROW_RECOVER, OVERRIDE_PLAN, PROMOTE_MODEL,
        PUBLISH_EXCHANGE, RELEASE_PRODUCT, SET_CONTROL_STATUS, SUBMIT_DETECTION, TASK_SENSOR,
        VIEW_PICTURE,
    };
    match role {
        Role::Administrator => true,
        // The security officer operates nothing (DN-22 §11, D-30): one action, and not
        // even the picture, because recovery is an offline act on a machine of its own.
        Role::SecurityOfficer => matches!(action, KEY_ESCROW_RECOVER),
        // Commander: the §4 rows are engagement acceptance at both layers, weapons
        // control status, hold or cease, plan apply, and product release. Identity
        // declaration per class and coverage-gap acceptance have no coarse action yet
        // and stay with GAP-058's per-class refinement.
        //
        // PUBLISH_EXCHANGE (GAP-065, signed by the owner the same day): granted alongside
        // RELEASE_PRODUCT on the judgment that whoever may mark a product releasable
        // should be who may send it, matching the §4 row this change added in
        // `docs/mission/roles-and-stakeholders.md`.
        Role::Commander => matches!(
            action,
            VIEW_PICTURE
                | DECIDE_PLAN
                | OVERRIDE_PLAN
                | SET_CONTROL_STATUS
                | APPLY_CONFIG
                | RELEASE_PRODUCT
                | PUBLISH_EXCHANGE
                | EXPORT_REPORT
        ),
        // Intelligence analyst: product release and reporting. Sensor tasking is a
        // *request* in §4, not authority, so TASK_SENSOR is deliberately absent.
        // PUBLISH_EXCHANGE joins RELEASE_PRODUCT here for the same reason it joins it
        // above (GAP-065, signed by the owner the same day).
        Role::IntelligenceAnalyst => {
            matches!(
                action,
                VIEW_PICTURE | EXPORT_REPORT | RELEASE_PRODUCT | PUBLISH_EXCHANGE
            )
        }
        // Planner: view only. §4 has no Planner row; rather than infer authority for a
        // role in an engagement chain, the owner set this to view-only on 2026-09-05.
        // The planning surfaces are read-and-draft, and there is no draft-plan action
        // in `actions` to grant. Widening this means adding the §4 row first.
        Role::Planner => matches!(action, VIEW_PICTURE),
        Role::Supervisor => matches!(
            action,
            VIEW_PICTURE
                | SUBMIT_DETECTION
                | DECIDE_PLAN
                | OVERRIDE_PLAN
                | TASK_SENSOR
                | EXPORT_REPORT
                | APPLY_CONFIG
        ),
        Role::Operator => matches!(action, VIEW_PICTURE | SUBMIT_DETECTION | DECIDE_PLAN),
        Role::SensorManager => matches!(action, VIEW_PICTURE | TASK_SENSOR | APPLY_CONFIG),
        Role::Analyst => matches!(action, VIEW_PICTURE | EXPORT_REPORT | PROMOTE_MODEL),
    }
}

/// Roles assigned at configuration time. Unknown operators have no role and may do
/// nothing.
#[derive(Debug, Default, Clone)]
pub struct StaticRoleAuthorizer {
    roles: HashMap<OperatorId, Role>,
}

impl StaticRoleAuthorizer {
    pub fn new(roles: impl IntoIterator<Item = (OperatorId, Role)>) -> Self {
        Self {
            roles: roles.into_iter().collect(),
        }
    }

    pub fn assign(&mut self, operator: OperatorId, role: Role) {
        self.roles.insert(operator, role);
    }
}

impl Authorizer for StaticRoleAuthorizer {
    fn role(&self, operator: OperatorId) -> Option<Role> {
        self.roles.get(&operator).copied()
    }

    fn can(&self, operator: OperatorId, action: &str) -> bool {
        self.role(operator).is_some_and(|r| role_permits(r, action))
    }
}

#[cfg(test)]
mod security_officer_tests {
    use super::*;

    #[test]
    fn the_security_officer_may_recover_and_do_nothing_else() {
        assert!(role_permits(
            Role::SecurityOfficer,
            actions::KEY_ESCROW_RECOVER
        ));
        for action in actions::ALL
            .iter()
            .filter(|a| **a != actions::KEY_ESCROW_RECOVER)
        {
            assert!(!role_permits(Role::SecurityOfficer, action), "{action}");
        }
        assert!(
            !role_permits(Role::Operator, actions::KEY_ESCROW_RECOVER),
            "nobody else recovers"
        );
        assert!(
            role_permits(Role::Administrator, actions::KEY_ESCROW_RECOVER),
            "administrators may do everything, as before"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operators_decide_but_do_not_override_or_configure() {
        let a = StaticRoleAuthorizer::new([(OperatorId(1), Role::Operator)]);
        assert!(a.can(OperatorId(1), actions::DECIDE_PLAN));
        assert!(!a.can(OperatorId(1), actions::OVERRIDE_PLAN));
        assert!(!a.can(OperatorId(1), actions::APPLY_CONFIG));
        assert!(matches!(
            a.require(OperatorId(1), actions::APPLY_CONFIG),
            Err(SecurityError::Forbidden(_))
        ));
    }

    /// GAP-042: the warned party's acknowledgement is an action a baseline may name, and
    /// it sits exactly where `EFFECTOR_REPORT` sits -- known to the build, and in no role
    /// row but the administrator's. Widening it means adding the row to
    /// `docs/mission/roles-and-stakeholders.md` §4 first, as every other row here says.
    #[test]
    fn acknowledging_a_warning_is_a_known_action_held_where_an_effector_report_is() {
        assert!(actions::is_known(actions::ACKNOWLEDGE_WARNING));
        assert!(role_permits(
            Role::Administrator,
            actions::ACKNOWLEDGE_WARNING
        ));
        for role in Role::ALL.iter().filter(|r| **r != Role::Administrator) {
            assert_eq!(
                role_permits(*role, actions::ACKNOWLEDGE_WARNING),
                role_permits(*role, actions::EFFECTOR_REPORT),
                "{role:?} holds one of the two reporting actions and not the other"
            );
        }
        assert_ne!(
            actions::ACKNOWLEDGE_WARNING,
            actions::ACKNOWLEDGE_HANDOVER,
            "a watch handover and a warned party are different acknowledgements"
        );
    }

    /// GAP-065 (signed by the owner the same day): `PUBLISH_EXCHANGE` sits with
    /// `RELEASE_PRODUCT` on `Commander` and `IntelligenceAnalyst` and nowhere else,
    /// matching the row `docs/mission/roles-and-stakeholders.md` §4 gained for it.
    #[test]
    fn publishing_to_exchange_is_held_wherever_product_release_is() {
        for role in Role::ALL.iter().copied() {
            assert_eq!(
                role_permits(role, actions::PUBLISH_EXCHANGE),
                role_permits(role, actions::RELEASE_PRODUCT),
                "{role:?} holds one of release and publish and not the other"
            );
        }
        assert!(role_permits(Role::Commander, actions::PUBLISH_EXCHANGE));
        assert!(role_permits(
            Role::IntelligenceAnalyst,
            actions::PUBLISH_EXCHANGE
        ));
        assert!(!role_permits(Role::Operator, actions::PUBLISH_EXCHANGE));
    }

    #[test]
    fn unknown_operator_may_do_nothing() {
        let a = StaticRoleAuthorizer::default();
        assert!(!a.can(OperatorId(9), actions::VIEW_PICTURE));
    }

    #[test]
    fn administrators_may_do_everything() {
        let a = StaticRoleAuthorizer::new([(OperatorId(2), Role::Administrator)]);
        assert!(a.can(OperatorId(2), actions::PROMOTE_MODEL));
        assert!(a.can(OperatorId(2), "some.future.action"));
    }
}
