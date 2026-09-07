// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! How this deployment decides: identification thresholds, staleness, weapons
//! control status, the authority matrix, decision timeouts, and baseline validity.
//!
//! Design: docs/design/DN-08-policy-configuration.md (signed 2026-09-05), with
//! `WeaponsControlStatus` from docs/design/DN-09-authority-and-control-status.md.
//! Capabilities CAP-5.6 and CAP-3.6.
//!
//! These types live here rather than in `gungnir-config` because `gungnir-policy`
//! and `gungnir-command` both read them and neither may depend on configuration
//! (agentic-coding-standards.md §1.1). `gungnir-config` deserializes into them.
//!
//! **Every setting has an explicit default and the defaults are the strictest
//! reading**, which is what keeps a partially configured deployment safe. The one
//! deliberate asymmetry: silence about authority denies, silence about expiry
//! preserves. Both errors are visible to the operator; only the first would be
//! unsafe.

use crate::{EffectorLayer, MissionTime};
use std::collections::BTreeMap;

/// Weapons control status, per effector layer, ordered most to least permissive.
///
/// `Hold` is the default, so a layer nobody configured does not become weapons-free
/// by omission. That single choice carries more safety weight than anything else in
/// docs/design/DN-09-authority-and-control-status.md.
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
pub enum WeaponsControlStatus {
    /// Engage anything not positively identified as friendly.
    Free,
    /// Engage only what is identified hostile, or what meets the declared criteria.
    Tight,
    /// Engage nothing without an explicit order.
    #[default]
    Hold,
}

impl WeaponsControlStatus {
    /// Parses the baseline's spelling; `None` for anything unrecognized, which the
    /// caller rejects rather than defaulting.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "free" => Some(WeaponsControlStatus::Free),
            "tight" => Some(WeaponsControlStatus::Tight),
            "hold" => Some(WeaponsControlStatus::Hold),
            _ => None,
        }
    }
}

/// Per-class confidence needed before the engine may declare an identity, and the
/// margin the leading hypothesis must hold over the runner-up. GAP-018 implements
/// against this.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct IdentificationSettings {
    /// Class name to minimum confidence, 0.0 to 1.0.
    #[serde(default)]
    pub thresholds: BTreeMap<String, f64>,
    #[serde(default)]
    pub minimum_margin: f64,
    /// Classes a person must confirm however confident the engine is.
    #[serde(default)]
    pub operator_confirms: Vec<String>,
}

impl IdentificationSettings {
    /// The threshold for a class, or `None` when none is configured.
    ///
    /// A class with no threshold is **not** given a guessed one: the caller treats
    /// it as operator-confirmed (docs/design/DN-08-policy-configuration.md §5).
    pub fn threshold_for(&self, class: &str) -> Option<f64> {
        self.thresholds.get(class).copied()
    }

    /// True when a person must confirm this class, either because it is listed or
    /// because no threshold is configured for it.
    pub fn requires_operator(&self, class: &str) -> bool {
        self.operator_confirms.iter().any(|c| c == class) || self.threshold_for(class).is_none()
    }
}

/// How long a track may go unobserved before it is drawn and treated as stale.
/// Per class, because a loitering surface craft and a cruise missile do not age at
/// the same rate. GAP-012 implements against this.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct StalenessSettings {
    pub default_s: f64,
    #[serde(default)]
    pub by_class_s: BTreeMap<String, f64>,
}

impl StalenessSettings {
    /// Seconds before a track of this class is stale: the class value, else the
    /// default.
    pub fn for_class(&self, class: &str) -> f64 {
        self.by_class_s
            .get(class)
            .copied()
            .unwrap_or(self.default_s)
    }
}

/// Weapons control status per effector layer.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct ControlStatusSettings {
    #[serde(default)]
    pub by_layer: BTreeMap<EffectorLayer, WeaponsControlStatus>,
}

impl ControlStatusSettings {
    /// The status in force for a layer. **An unconfigured layer is at `Hold`.**
    pub fn for_layer(&self, layer: EffectorLayer) -> WeaponsControlStatus {
        self.by_layer
            .get(&layer)
            .copied()
            .unwrap_or(WeaponsControlStatus::Hold)
    }
}

/// One row of the authority matrix in
/// docs/mission/roles-and-stakeholders.md §4, as configuration.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AuthorityRule {
    /// A constant from `gungnir_security::actions`; validated at load, because a
    /// misspelled action grants nothing and looks like a grant.
    pub action: String,
    /// A role name from the adopted set (decision D-05).
    pub role: String,
    #[serde(default)]
    pub layer: Option<EffectorLayer>,
    #[serde(default)]
    pub class: Option<String>,
    /// Pre-delegated cases per decision D-15. `false` means the role decides case
    /// by case.
    #[serde(default)]
    pub pre_delegated: bool,
}

impl AuthorityRule {
    /// How specific this rule is: class and layer, then layer, then class, then
    /// neither. Most specific wins
    /// (docs/design/DN-09-authority-and-control-status.md §5).
    pub fn specificity(&self) -> u8 {
        u8::from(self.layer.is_some()) + u8::from(self.class.is_some())
    }

    fn matches(&self, action: &str, layer: Option<EffectorLayer>, class: Option<&str>) -> bool {
        if self.action != action {
            return false;
        }
        if let Some(required) = self.layer {
            if layer != Some(required) {
                return false;
            }
        }
        if let Some(required) = &self.class {
            if class != Some(required.as_str()) {
                return false;
            }
        }
        true
    }
}

/// The authority matrix.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct AuthoritySettings {
    #[serde(default)]
    pub rules: Vec<AuthorityRule>,
}

impl AuthoritySettings {
    /// The most specific rule granting `role` this action, if any.
    ///
    /// **No matching rule means denied.** An action nobody is granted is an action
    /// nobody may take (docs/design/DN-08-policy-configuration.md §5).
    pub fn rule_for(
        &self,
        action: &str,
        role: &str,
        layer: Option<EffectorLayer>,
        class: Option<&str>,
    ) -> Option<&AuthorityRule> {
        self.rules
            .iter()
            .filter(|r| r.role == role && r.matches(action, layer, class))
            .max_by_key(|r| r.specificity())
    }

    /// Whether `role` may take this action here at all.
    pub fn permits(
        &self,
        action: &str,
        role: &str,
        layer: Option<EffectorLayer>,
        class: Option<&str>,
    ) -> bool {
        self.rule_for(action, role, layer, class).is_some()
    }
}

/// Timeouts and escalation for pending decisions. DN-10 implements against this.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct DecisionSettings {
    #[serde(default)]
    pub expiry_s: BTreeMap<EffectorLayer, f64>,
    #[serde(default)]
    pub escalate_after_s: BTreeMap<EffectorLayer, f64>,
}

impl DecisionSettings {
    /// Seconds before a pending decision at this layer expires.
    ///
    /// `None` means no expiry is configured, and the item **never expires**. That
    /// is the deliberate asymmetry: silently discarding a decision nobody took
    /// would lose information, so silence preserves rather than denies. The queue
    /// shows which items have no expiry and why.
    pub fn expiry_for(&self, layer: EffectorLayer) -> Option<f64> {
        self.expiry_s.get(&layer).copied()
    }

    pub fn escalate_after_for(&self, layer: EffectorLayer) -> Option<f64> {
        self.escalate_after_s.get(&layer).copied()
    }
}

/// Limits on fires tasks (docs/design/DN-05-fires.md §6).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FiresSettings {
    /// Largest target location error, metres, a fires task may be recommended at.
    pub max_location_error_m: f64,
    /// Minimum separation, metres, from friendly positions and no-fire areas.
    pub minimum_separation_m: f64,
}

impl Default for FiresSettings {
    fn default() -> Self {
        Self {
            max_location_error_m: 100.0,
            minimum_separation_m: 500.0,
        }
    }
}

/// When a baseline is valid.
///
/// A baseline outside its window may be read, replayed, and inspected; it may not
/// be promoted, and a plan produced under a baseline that has since expired is
/// marked superseded rather than silently applied. Expiry never changes a picture
/// retroactively.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ValidityWindow {
    pub valid_from: MissionTime,
    pub valid_until: Option<MissionTime>,
}

impl ValidityWindow {
    pub fn contains(&self, now: MissionTime) -> bool {
        if now < self.valid_from {
            return false;
        }
        match self.valid_until {
            Some(until) => now <= until,
            None => true,
        }
    }
}

/// Everything about how this deployment decides, in one versioned place.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct PolicySettings {
    #[serde(default)]
    pub identification: IdentificationSettings,
    #[serde(default)]
    pub staleness: StalenessSettings,
    #[serde(default)]
    pub control_status: ControlStatusSettings,
    #[serde(default)]
    pub authority: AuthoritySettings,
    #[serde(default)]
    pub decisions: DecisionSettings,
    #[serde(default)]
    pub fires: FiresSettings,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(
        action: &str,
        role: &str,
        layer: Option<EffectorLayer>,
        class: Option<&str>,
    ) -> AuthorityRule {
        AuthorityRule {
            action: action.into(),
            role: role.into(),
            layer,
            class: class.map(str::to_string),
            pre_delegated: false,
        }
    }

    #[test]
    fn an_unconfigured_layer_is_at_hold() {
        let s = ControlStatusSettings::default();
        assert_eq!(
            s.for_layer(EffectorLayer::Area),
            WeaponsControlStatus::Hold,
            "silence about authority must deny"
        );
        assert_eq!(
            s.for_layer(EffectorLayer::Point),
            WeaponsControlStatus::Hold
        );
    }

    #[test]
    fn an_action_with_no_rule_is_denied() {
        let a = AuthoritySettings::default();
        assert!(!a.permits("plan.decide", "operator", None, None));
    }

    #[test]
    fn the_most_specific_matching_rule_wins() {
        let a = AuthoritySettings {
            rules: vec![
                rule("plan.decide", "operator", None, None),
                rule("plan.decide", "operator", Some(EffectorLayer::Point), None),
                rule(
                    "plan.decide",
                    "operator",
                    Some(EffectorLayer::Point),
                    Some("uas"),
                ),
            ],
        };
        let chosen = a
            .rule_for(
                "plan.decide",
                "operator",
                Some(EffectorLayer::Point),
                Some("uas"),
            )
            .expect("a rule matches");
        assert_eq!(chosen.specificity(), 2);

        // A layer with no class-specific rule falls back to the layer rule.
        let chosen = a
            .rule_for(
                "plan.decide",
                "operator",
                Some(EffectorLayer::Point),
                Some("air"),
            )
            .expect("a rule matches");
        assert_eq!(chosen.specificity(), 1);
    }

    #[test]
    fn a_rule_for_another_role_does_not_grant() {
        let a = AuthoritySettings {
            rules: vec![rule(
                "plan.decide",
                "supervisor",
                Some(EffectorLayer::Area),
                None,
            )],
        };
        assert!(a.permits("plan.decide", "supervisor", Some(EffectorLayer::Area), None));
        assert!(!a.permits("plan.decide", "operator", Some(EffectorLayer::Area), None));
    }

    #[test]
    fn silence_about_expiry_preserves_rather_than_discarding() {
        let d = DecisionSettings::default();
        assert_eq!(
            d.expiry_for(EffectorLayer::Point),
            None,
            "no expiry configured means the item never expires"
        );
    }

    #[test]
    fn an_unthresholded_class_needs_a_person() {
        let mut i = IdentificationSettings::default();
        assert!(i.requires_operator("airliner"));
        i.thresholds.insert("airliner".into(), 0.9);
        assert!(!i.requires_operator("airliner"));
        i.operator_confirms.push("airliner".into());
        assert!(
            i.requires_operator("airliner"),
            "an explicit listing still wins"
        );
    }

    #[test]
    fn staleness_falls_back_to_the_default() {
        let mut s = StalenessSettings {
            default_s: 30.0,
            by_class_s: BTreeMap::new(),
        };
        assert!((s.for_class("uas") - 30.0).abs() < f64::EPSILON);
        s.by_class_s.insert("uas".into(), 5.0);
        assert!((s.for_class("uas") - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_validity_window_is_inclusive_and_open_ended_when_it_has_no_end() {
        let w = ValidityWindow {
            valid_from: MissionTime(100.0),
            valid_until: Some(MissionTime(200.0)),
        };
        assert!(!w.contains(MissionTime(99.0)));
        assert!(w.contains(MissionTime(100.0)));
        assert!(w.contains(MissionTime(200.0)));
        assert!(!w.contains(MissionTime(201.0)));

        let open = ValidityWindow {
            valid_from: MissionTime(100.0),
            valid_until: None,
        };
        assert!(open.contains(MissionTime(1_000_000.0)));
    }

    #[test]
    fn settings_round_trip_through_serde_with_layer_keys() {
        let mut s = PolicySettings::default();
        s.control_status
            .by_layer
            .insert(EffectorLayer::Point, WeaponsControlStatus::Tight);
        s.decisions.expiry_s.insert(EffectorLayer::Area, 45.0);
        let json = serde_json::to_string(&s).expect("serialize");
        let back: PolicySettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(s, back);
        assert_eq!(
            back.control_status.for_layer(EffectorLayer::Point),
            WeaponsControlStatus::Tight
        );
    }
}
